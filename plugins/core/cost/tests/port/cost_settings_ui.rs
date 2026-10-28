//! Twins of ilium-client/src/cost_settings_ui.rs: the Cost settings page, its hit-testing and its dialogs.
//!
//! The page is asked through `settings-ui.view` (`{settings, width}` answering `{page, rows:[{row,first_line,
//! control_x,control_line}], bars, bar_widths}`), `settings-ui.hit` (`{settings, width, x, y}` answering
//! `{row, direction}|null`), `settings-ui.value-hit` and the interaction ops named per test.

use serde_json::{json, Value};

use crate::common::client;

fn view(settings: Value, width: u64) -> Value {
    client(
        "settings-ui.view",
        json!({ "settings": settings, "width": width }),
    )
}

fn page(settings: Value, width: u64) -> String {
    view(settings, width)["page"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

fn span_of(view: &Value, row: &str) -> Value {
    view["rows"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|span| span["row"] == json!(row))
        .unwrap_or(Value::Null)
}

fn hit(settings: Value, width: u64, x: u64, y: u64) -> Value {
    client(
        "settings-ui.hit",
        json!({ "settings": settings, "width": width, "height": 200, "x": x, "y": y }),
    )
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::clicking_a_metric_control_steps_it_and_the_window_row_toggles
#[test]
fn port_cost_settings_ui_clicking_a_metric_control_steps_it_and_the_window_row_toggles() {
    let reply = client(
        "settings-ui.value-hit",
        json!({ "settings": {}, "width": 110, "height": 60, "row": "metric:dollars", "at": "value", "button": "left" }),
    );
    assert_eq!(reply["action"], json!("next_choice"));

    let quota = json!({ "metric": "quota" });
    let view_value = view(quota.clone(), 110);
    let window = span_of(&view_value, "quota_window");
    let x = window["control_x"].as_u64().unwrap_or(0) + 4;
    let click = hit(quota, 110, x, window["control_line"].as_u64().unwrap_or(0));
    assert_eq!(click["row"], json!("quota_window"));
    // The window is a toggle, not a stepper.
    assert_eq!(click["direction"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::clicking_cost_number_star_opens_keyboard_entry_dialog
#[test]
fn port_cost_settings_ui_clicking_cost_number_star_opens_keyboard_entry_dialog() {
    let reply = client(
        "settings-ui.number-dialog",
        json!({ "settings": { "calibration": "budget" }, "row": "budget", "click": "open", "draft_keys": "17.125", "confirm": true }),
    );
    assert_eq!(reply["dialog_kind"], json!("number"));
    assert_eq!(reply["target_row"], json!("budget"));
    assert_eq!(reply["settings"]["budget_usd"], json!(17.125));
    assert_eq!(reply["persisted_cost"], reply["settings"]);
    assert_eq!(reply["mode_after"], json!("settings"));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::clicks_toggle_options_and_step_parameters
#[test]
fn port_cost_settings_ui_clicks_toggle_options_and_step_parameters() {
    let view_value = view(json!({}), 110);
    let meter = span_of(&view_value, "display:meter");
    let click = hit(
        json!({}),
        110,
        40,
        meter["first_line"].as_u64().unwrap_or(0),
    );
    assert_eq!(
        (click["row"].clone(), click["direction"].clone()),
        (json!("display:meter"), json!(0))
    );

    let visibility = span_of(&view_value, "visibility:meter");
    assert_eq!(
        hit(
            json!({}),
            110,
            3,
            visibility["first_line"].as_u64().unwrap_or(0)
        ),
        Value::Null
    );
    let click = hit(
        json!({}),
        110,
        visibility["control_x"].as_u64().unwrap_or(0),
        visibility["control_line"].as_u64().unwrap_or(0),
    );
    assert_eq!(click["row"], json!("visibility:meter"));

    let window = span_of(&view_value, "sparkline_window");
    let (cx, cy) = (
        window["control_x"].as_u64().unwrap_or(0),
        window["control_line"].as_u64().unwrap_or(0),
    );
    let left = hit(json!({}), 110, cx, cy);
    let right = hit(json!({}), 110, cx + 6, cy);
    assert_eq!(
        (left["direction"].clone(), right["direction"].clone()),
        (json!(-1), json!(1))
    );
    // Description lines do not step the value.
    assert_eq!(hit(json!({}), 110, cx, cy + 1), Value::Null);

    let selector = span_of(&view_value, "calibration:own_history");
    let click = hit(
        json!({}),
        110,
        selector["control_x"].as_u64().unwrap_or(0) + 1,
        selector["control_line"].as_u64().unwrap_or(0),
    );
    assert_eq!(click["row"], json!("calibration:own_history"));
    assert_eq!(click["direction"], json!(1));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::cost_choice_right_click_reverses_and_plus_opens_the_complete_dialog
#[test]
fn port_cost_settings_ui_cost_choice_right_click_reverses_and_plus_opens_the_complete_dialog() {
    let reply = client(
        "settings-ui.choice-dialog",
        json!({ "settings": { "metric": "dollars" }, "row": "metric", "right_click_value": true, "click": "open", "pick_first_option": true }),
    );
    // Right click steps back through the catalog, wrapping from the first entry to the last.
    assert_eq!(reply["metric_after_right_click"], reply["previous_metric"]);
    assert_eq!(reply["dialog_kind"], json!("choice"));
    assert_eq!(reply["target_row"], json!("metric"));
    assert_eq!(reply["option_count"], reply["metric_catalog_len"]);
    assert_eq!(reply["settings"]["metric"], json!("dollars"));
    assert_eq!(reply["persisted_cost"]["metric"], json!("dollars"));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::cost_settings_sections_use_rounded_title_bars_without_widening_the_page
#[test]
fn port_cost_settings_ui_cost_settings_sections_use_rounded_title_bars_without_widening_the_page() {
    for width in [110u64, 28] {
        let reply = view(json!({}), width);
        let bars = reply["bars"].as_array().cloned().unwrap_or_default();
        assert_eq!(bars.len(), 4);
        for bar in &bars {
            let text = bar["text"].as_str().unwrap_or("");
            assert!(text.starts_with("╭─") && text.ends_with('╮'), "{text}");
            assert!(bar["width"].as_u64().unwrap_or(999) <= width);
        }
        if width >= 110 {
            for title in [
                "WHAT IS MEASURED?",
                "HOW IS \"EXPENSIVE\" DECIDED?",
                "WHAT TO SHOW ON EACH AGENT",
                "SPARKLINE AND ORDER",
            ] {
                assert!(
                    bars.iter()
                        .any(|bar| bar["text"].as_str().unwrap_or("").contains(title)),
                    "{title}"
                );
            }
        }
    }
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::every_display_option_has_a_checkbox_visibility_and_sample
#[test]
fn port_cost_settings_ui_every_display_option_has_a_checkbox_visibility_and_sample() {
    let text = page(json!({}), 110);
    for label in client("settings-ui.display-labels", json!({}))["labels"]
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        let label = label.as_str().unwrap_or("");
        assert!(text.contains(label), "{label}");
    }
    assert!(text.contains("[x] Five-cell meter"));
    assert!(text.contains("[ ] Level glyph"));
    assert!(text.contains("← Only when hovering the entry + →"));
    assert!(text.contains("Sparkline window"));
    assert!(
        text.contains("- 6 h + *"),
        "default window is six hours: {text}"
    );
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::metric_selector_switches_every_unit_and_quota_window
#[test]
fn port_cost_settings_ui_metric_selector_switches_every_unit_and_quota_window() {
    let text = page(json!({}), 110);
    assert!(text.contains("WHAT IS MEASURED?"));
    assert!(text.contains("← API dollars + →"));
    assert!(!text.contains("Quota window"));
    assert!(text.contains("<$1.00"), "dollar ladders by default");

    let quota = json!({ "metric": "quota" });
    let quota_view = view(quota, 110);
    let text = quota_view["page"].as_str().unwrap_or("").to_string();
    assert!(text.contains("← Plan quota + →"));
    assert!(text.contains("← Short window + →"));
    assert!(text.contains("<1.0%"), "fixed-band ladder is in percent");
    assert!(!text.contains("<$1.00"));
    assert!(text.contains("Claude Code transcripts carry no quota"));
    let metric_rows = quota_view["rows"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|span| {
            let row = span["row"].as_str().unwrap_or("");
            row.starts_with("metric:") || row == "quota_window"
        })
        .count();
    assert_eq!(metric_rows, 2);

    let text = page(json!({ "metric": "quota", "calibration": "budget" }), 110);
    assert!(
        text.contains("- 10% + *"),
        "budget is a share of the window"
    );
    assert!(text.contains("quota_budget_percent"));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::page_explains_every_calibration_and_shows_live_ladders
#[test]
fn port_cost_settings_ui_page_explains_every_calibration_and_shows_live_ladders() {
    let text = page(json!({}), 110);
    assert!(text.contains("← Relative to your history + →"));
    assert!(
        text.contains("<$1"),
        "fixed ladder starts below the first cut: {text}"
    );
    assert!(text.contains("≥$50"));
    assert!(text.contains("History window"));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::row_spans_cover_every_row_once_in_order
#[test]
fn port_cost_settings_ui_row_spans_cover_every_row_once_in_order() {
    let reply = view(json!({}), 110);
    let mut found: Vec<String> = reply["rows"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|span| span["row"].as_str().unwrap_or("").to_string())
        .collect();
    let mut expected: Vec<String> = client("settings.rows", json!({ "settings": {} }))["rows"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|row| row.as_str().unwrap_or("").to_string())
        .collect();
    found.sort();
    expected.sort();
    assert_eq!(found, expected);
    let lines: Vec<u64> = reply["rows"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|span| span["first_line"].as_u64().unwrap_or(0))
        .collect();
    assert!(lines.windows(2).all(|pair| pair[0] < pair[1]));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::scroll_follows_the_selection_in_both_directions
#[test]
fn port_cost_settings_ui_scroll_follows_the_selection_in_both_directions() {
    let down = client(
        "settings-ui.scroll-for-selection",
        json!({ "settings": {}, "width": 100, "height": 12, "selected": "last", "scroll": 0 }),
    );
    let down = down["scroll"].as_u64().unwrap_or(0);
    assert!(down > 0);
    let up = client(
        "settings-ui.scroll-for-selection",
        json!({ "settings": {}, "width": 100, "height": 12, "selected": 0, "scroll": down }),
    );
    assert!(up["scroll"].as_u64().unwrap_or(u64::MAX) < down);
    let max = client(
        "settings-ui.max-scroll",
        json!({ "settings": {}, "width": 100, "height": 12 }),
    );
    assert!(max["max_scroll"].as_u64().unwrap_or(0) > 0);
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::selecting_a_calibration_reveals_its_parameter_row
#[test]
fn port_cost_settings_ui_selecting_a_calibration_reveals_its_parameter_row() {
    assert!(!page(json!({}), 110).contains("Budget per agent"));
    let text = page(json!({ "calibration": "budget" }), 110);
    assert!(text.contains("Budget per agent"));
    assert!(text.contains("- $10 + *"));
    assert!(!text.contains("History window"));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::shared_cost_chrome_renders_and_hits_every_numeric_and_choice_variant
#[test]
fn port_cost_settings_ui_shared_cost_chrome_renders_and_hits_every_numeric_and_choice_variant() {
    for metric in ["dollars", "quota"] {
        for calibration in [
            "own_history",
            "fixed_bands",
            "budget",
            "burn_rate",
            "peer_relative",
        ] {
            let reply = client(
                "settings-ui.chrome-check",
                json!({ "settings": { "metric": metric, "calibration": calibration }, "width": 110 }),
            );
            // Every numeric row draws the decrement, increment and star glyphs where its geometry says and
            // hits Decrement/Increment/EditNumber; every choice row draws ← → + and hits NextChoice,
            // PreviousChoice (right button) and OpenChoices.
            assert_eq!(
                reply["mismatches"],
                json!([]),
                "{metric} {calibration}: {reply}"
            );
            assert!(
                reply["controls_checked"].as_u64().unwrap_or(0) > 0,
                "{metric} {calibration}"
            );
        }
    }
}

/// REQ: PORT:ilium-client/src/keys.rs::cost_tab_keyboard_selects_toggles_steps_and_scrolls_the_selection_into_view
#[test]
fn port_cost_settings_ui_cost_tab_keyboard_selects_toggles_steps_and_scrolls_the_selection_into_view(
) {
    // Assumed op `settings-ui.keyboard`: replays chords against the page and answers the settings and scroll.
    let reply = client(
        "settings-ui.keyboard",
        json!({ "settings": {}, "width": 110, "height": 12, "select": "calibration", "chords": ["right"] }),
    );
    assert_eq!(reply["settings"]["calibration"], json!("budget"));
    assert_eq!(reply["row_visible"]["budget"], json!(true));

    let reply = client(
        "settings-ui.keyboard",
        json!({ "settings": {}, "width": 110, "height": 12, "select": "sparkline", "chords": ["space"] }),
    );
    assert_eq!(reply["settings"]["sparkline"], json!(true));

    let reply = client(
        "settings-ui.keyboard",
        json!({ "settings": {}, "width": 110, "height": 12, "select": "visibility:sparkline", "chords": ["right"] }),
    );
    assert_eq!(
        reply["settings"]["visibility"]["sparkline"],
        json!("always")
    );

    let window = |chords: Value| {
        client(
            "settings-ui.keyboard",
            json!({ "settings": {}, "width": 110, "height": 12, "select": "sparkline_window", "chords": chords }),
        )
    };
    assert_eq!(
        window(json!(["right"]))["settings"]["sparkline_window_minutes"],
        json!(480)
    );
    assert_eq!(
        window(json!(["left", "left"]))["settings"]["sparkline_window_minutes"],
        json!(240)
    );
    let scrolled = window(json!([]));
    assert!(
        scrolled["scroll"].as_u64().is_some_and(|scroll| scroll > 0),
        "the selected row is scrolled into view: {scrolled}"
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::cost_window_accepts_an_exact_typed_value_and_keeps_the_field_open_on_a_typo
#[test]
fn port_cost_settings_ui_cost_window_accepts_an_exact_typed_value_and_keeps_the_field_open_on_a_typo(
) {
    let dialog = |draft_keys: &str, confirm: bool| {
        client(
            "settings-ui.number-dialog",
            json!({ "settings": {}, "row": "sparkline_window", "click": "open", "draft_keys": draft_keys, "confirm": confirm }),
        )
    };
    // Opening shows the current value as the draft.
    let opened = dialog("", false);
    assert_eq!(opened["draft"], json!("360"));
    // A typo keeps the field open with the typed draft, an error and the stored value.
    let typo = dialog("<bs><bs><bs>5x", true);
    assert_eq!(typo["draft"], json!("5x"));
    assert_eq!(typo["dialog_kind"], json!("number"));
    assert!(
        typo["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty()),
        "{typo}"
    );
    assert_eq!(typo["settings"]["sparkline_window_minutes"], json!(360));
    // An exact value saves 10 hours as 600 minutes and keeps Settings open.
    let saved = dialog("<bs><bs>10h", true);
    assert_eq!(saved["settings"]["sparkline_window_minutes"], json!(600));
    assert_eq!(saved["mode_after"], json!("settings"));
    // Escape cancels and leaves the value.
    let cancelled = client(
        "settings-ui.number-dialog",
        json!({ "settings": { "sparkline_window_minutes": 600 }, "row": "sparkline_window", "click": "open", "draft_keys": "9", "cancel": true }),
    );
    assert_eq!(
        cancelled["settings"]["sparkline_window_minutes"],
        json!(600)
    );
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::clicking_a_metric_control_steps_it_and_the_window_row_toggles
#[test]
fn port_cost_settings_ui_right_click_on_a_metric_value_steps_back() {
    let reply = client(
        "settings-ui.value-hit",
        json!({ "settings": {}, "width": 110, "height": 60, "row": "metric:dollars", "at": "value", "button": "right" }),
    );
    assert_eq!(reply["action"], json!("previous_choice"));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::clicking_cost_number_star_opens_keyboard_entry_dialog
#[test]
fn port_cost_settings_ui_malformed_typed_budget_keeps_the_stored_budget() {
    let reply = client(
        "settings-ui.number-dialog",
        json!({ "settings": { "calibration": "budget" }, "row": "budget", "click": "open", "draft_keys": "abc", "confirm": true }),
    );
    assert_eq!(reply["dialog_kind"], json!("number"));
    assert!(reply["error"].as_str().is_some_and(|error| !error.is_empty()));
    assert_eq!(reply["settings"]["budget_usd"], json!(10.0));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::clicks_toggle_options_and_step_parameters
#[test]
fn port_cost_settings_ui_clicking_a_visibility_control_toggles_without_stepping() {
    let view_value = view(json!({}), 110);
    let visibility = span_of(&view_value, "visibility:meter");
    let click = hit(
        json!({}),
        110,
        visibility["control_x"].as_u64().unwrap_or(0),
        visibility["control_line"].as_u64().unwrap_or(0),
    );
    assert_eq!(click["row"], json!("visibility:meter"));
    assert_eq!(click["direction"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::cost_choice_right_click_reverses_and_plus_opens_the_complete_dialog
#[test]
fn port_cost_settings_ui_choice_dialog_lists_at_least_two_metrics() {
    let reply = client(
        "settings-ui.choice-dialog",
        json!({ "settings": { "metric": "dollars" }, "row": "metric", "right_click_value": false, "click": "open", "pick_first_option": false }),
    );
    assert_eq!(reply["dialog_kind"], json!("choice"));
    assert!(reply["option_count"].as_u64().unwrap_or(0) >= 2);
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::cost_settings_sections_use_rounded_title_bars_without_widening_the_page
#[test]
fn port_cost_settings_ui_quota_page_keeps_four_title_bars() {
    let reply = view(json!({ "metric": "quota" }), 110);
    assert_eq!(reply["bars"].as_array().map(Vec::len), Some(4));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::every_display_option_has_a_checkbox_visibility_and_sample
#[test]
fn port_cost_settings_ui_only_the_enabled_display_option_is_ticked_by_default() {
    let text = page(json!({}), 110);
    assert_eq!(text.matches("[x]").count(), 1, "{text}");
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::metric_selector_switches_every_unit_and_quota_window
#[test]
fn port_cost_settings_ui_quota_metric_names_its_window_row() {
    let text = page(json!({ "metric": "quota" }), 110);
    assert!(text.contains("Quota window"), "{text}");
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::page_explains_every_calibration_and_shows_live_ladders
#[test]
fn port_cost_settings_ui_burn_rate_page_has_no_history_window_row() {
    let text = page(json!({ "calibration": "burn_rate" }), 110);
    assert!(!text.contains("History window"), "{text}");
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::row_spans_cover_every_row_once_in_order
#[test]
fn port_cost_settings_ui_narrow_page_has_the_same_row_count_as_wide_page() {
    let wide = view(json!({}), 110);
    let narrow = view(json!({}), 28);
    assert_eq!(
        wide["rows"].as_array().map(Vec::len),
        narrow["rows"].as_array().map(Vec::len)
    );
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::scroll_follows_the_selection_in_both_directions
#[test]
fn port_cost_settings_ui_tall_panel_needs_no_scrolling() {
    let max = client(
        "settings-ui.max-scroll",
        json!({ "settings": {}, "width": 100, "height": 200 }),
    );
    assert_eq!(max["max_scroll"], json!(0));
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::selecting_a_calibration_reveals_its_parameter_row
#[test]
fn port_cost_settings_ui_quota_budget_parameter_row_is_shown_as_percent() {
    let text = page(json!({ "metric": "quota", "calibration": "budget" }), 110);
    assert!(text.contains("- 10% + *"), "{text}");
}

/// REQ: PORT:ilium-client/src/cost_settings_ui.rs::shared_cost_chrome_renders_and_hits_every_numeric_and_choice_variant
#[test]
fn port_cost_settings_ui_chrome_check_holds_at_narrow_width() {
    let reply = client(
        "settings-ui.chrome-check",
        json!({ "settings": { "metric": "quota", "calibration": "fixed_bands" }, "width": 28 }),
    );
    assert_eq!(reply["mismatches"], json!([]), "{reply}");
}

/// REQ: PORT:ilium-client/src/keys.rs::cost_tab_keyboard_selects_toggles_steps_and_scrolls_the_selection_into_view
#[test]
fn port_cost_settings_ui_keyboard_right_then_left_returns_the_sparkline_window() {
    let reply = client(
        "settings-ui.keyboard",
        json!({ "settings": {}, "width": 110, "height": 12, "select": "sparkline_window", "chords": ["right", "left"] }),
    );
    assert_eq!(
        reply["settings"]["sparkline_window_minutes"],
        json!(360)
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::cost_window_accepts_an_exact_typed_value_and_keeps_the_field_open_on_a_typo
#[test]
fn port_cost_settings_ui_typed_days_save_as_minutes() {
    let saved = client(
        "settings-ui.number-dialog",
        json!({ "settings": {}, "row": "sparkline_window", "click": "open", "draft_keys": "<bs><bs>2d", "confirm": true }),
    );
    assert_eq!(saved["settings"]["sparkline_window_minutes"], json!(2880));
    assert_eq!(saved["mode_after"], json!("settings"));
}
