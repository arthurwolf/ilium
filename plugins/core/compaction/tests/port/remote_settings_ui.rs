//! Twins of the inline tests in ilium-client/src/remote_compaction_settings_ui.rs (the
//! Remote compaction settings page: text, hit testing, geometry, scrolling, help anchors).
//! Gaps: `settings_page_text`, `settings_privacy_banner_text`, `settings_hit`,
//! `settings_row_geometry`, `settings_scroll_for_selection`, `settings_max_scroll`,
//! `settings_help_anchors`, `settings_banner_close_rect` (see ../API_GAPS_port.md).

use serde_json::json;

use ilium_plugin_sdk::testing::FakeHost;

use crate::common::*;

fn tab() -> FakeHost {
    let mut host = client_host();
    host.open_settings_tab("remote-compaction");
    host
}

fn dismissed() -> FakeHost {
    let mut host = tab();
    host.settings_dismiss_banner();
    host.settle();
    host
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::banner_text_names_the_configured_provider_and_model
#[test]
fn rsui_banner_text_names_the_configured_provider_and_model() {
    let host = tab();
    let banner = host.settings_privacy_banner_text(
        json!({"provider": "Anthropic", "model": "claude-test-model"}),
    );
    assert!(banner.contains("Anthropic / claude-test-model"), "{banner}");
    assert!(banner.contains("privacy decision"));
    assert!(banner.contains("transcript"));
    assert!(host
        .settings_privacy_banner_text(json!({"provider": null, "model": null}))
        .contains("(no model selected)"));
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::the_tab_shows_the_banner_until_it_is_dismissed
#[test]
fn rsui_the_tab_shows_the_banner_until_it_is_dismissed() {
    let mut host = tab();
    let page = host.settings_page_text(110);
    assert!(page.contains("Privacy decision"), "{page}");
    assert!(page.contains("[x]"));
    assert!(page.contains("sends this session's transcript"));
    assert!(page.contains("Kilo Gateway"));
    assert_eq!(
        host.settings_rows().first().map(String::as_str),
        Some("privacy_banner")
    );
    host.settings_dismiss_banner();
    host.settle();
    let page = host.settings_page_text(110);
    assert!(!page.contains("Privacy decision"), "{page}");
    assert!(!page.contains("sends this session's transcript"));
    assert_eq!(
        host.settings_rows().first().map(String::as_str),
        Some("enabled")
    );
    assert!(!host.settings_banner_visible());
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::the_page_lists_every_section_row_and_technique_description
#[test]
fn rsui_the_page_lists_every_section_row_and_technique_description() {
    let host = dismissed();
    let page = host.settings_page_text(120);
    for heading in host.settings_section_headings() {
        assert!(page.contains(&heading), "{heading}");
    }
    for label in host.settings_row_labels() {
        assert!(page.contains(&label), "{label}");
    }
    for expected in [
        "[ ] Remote compaction",
        "[x] Redact secrets",
        "- 65% + *",
        "\u{2190} Claude Code + \u{2192}",
        "\u{2190} Codex + \u{2192}",
        "the real upstream prompt",
        "- 20k tokens + *",
        "- 2 min + *",
        "Kilo Gateway / ",
        "Change it there",
    ] {
        assert!(page.contains(expected), "missing {expected:?} in\n{page}");
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::technique_rows_show_provenance_for_each_technique
#[test]
fn rsui_technique_rows_show_provenance_for_each_technique() {
    let mut host = dismissed();
    host.settings_set_value("technique:claude", json!("ilium"));
    let page = host.settings_page_text(120);
    assert!(page.contains("Ilium's own design"), "{page}");
    host.settings_set_value("technique:claude", json!("custom"));
    let page = host.settings_page_text(120);
    assert!(page.contains("Your own instructions"), "{page}");
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::custom_prompt_row_previews_the_text_and_its_length
#[test]
fn rsui_custom_prompt_row_previews_the_text_and_its_length() {
    let mut host = dismissed();
    assert!(host.settings_page_text(120).contains("Not set"));
    host.settings_commit_prompt("Keep the migration plan.\nSecond line");
    let page = host.settings_page_text(120);
    assert!(page.contains("Keep the migration plan."), "{page}");
    assert!(page.contains("(35 characters)"), "{page}");
    assert!(!page.contains("Second"));
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::rendered_page_and_dialog_banner_draw_and_report_the_close_button
#[test]
fn rsui_rendered_page_and_dialog_banner_draw_and_report_the_close_button() {
    let mut host = tab();
    let drawn = host.settings_page_text(110);
    assert!(drawn.contains("Privacy decision"), "{drawn}");
    assert!(drawn.contains("Remote compaction"));
    let close = host
        .settings_banner_close_rect()
        .expect("close button published");
    assert_eq!(close.row, "privacy_banner");
    assert_eq!(close.text, "[x]");
    assert_eq!((close.height, close.width), (1, 3));
    host.settings_dismiss_banner();
    host.settle();
    assert!(host.settings_banner_close_rect().is_none());
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::clicks_resolve_to_toggles_steppers_the_banner_close_button_and_inert_text
#[test]
fn rsui_clicks_resolve_to_toggles_steppers_the_banner_close_button_and_inert_text() {
    let mut host = tab();
    let banner = host.settings_row_geometry("privacy_banner");
    let close = host
        .settings_hit(banner.control_column + 1, banner.control_line)
        .expect("hit");
    assert_eq!(
        (close.action.as_str(), close.row.as_str()),
        ("adjust:0", "privacy_banner")
    );
    let elsewhere = host
        .settings_hit(banner.box_column + 6, banner.first_line + 1)
        .expect("hit");
    assert_eq!(elsewhere.action, "select");
    host.settings_dismiss_banner();
    host.settle();
    let enabled = host.settings_row_geometry("enabled");
    let toggle = host
        .settings_hit(enabled.box_column + 8, enabled.first_line)
        .expect("hit");
    assert_eq!(toggle.action, "adjust:0");
    assert_eq!(toggle.row, "enabled");
    let threshold = host.settings_row_geometry("threshold");
    assert_eq!(
        host.settings_hit(threshold.next_column, threshold.control_line)
            .unwrap()
            .action,
        "adjust:1"
    );
    assert_eq!(
        host.settings_hit(threshold.previous_column, threshold.control_line)
            .unwrap()
            .action,
        "adjust:-1"
    );
    assert_eq!(
        host.settings_hit(threshold.open_column, threshold.control_line)
            .unwrap()
            .action,
        "open"
    );
    assert_eq!(
        host.settings_hit(threshold.control_column + 4, threshold.control_line + 1)
            .unwrap()
            .action,
        "select"
    );
    let claude = host.settings_row_geometry("technique:claude");
    assert_eq!(
        host.settings_hit(claude.next_column, claude.control_line)
            .unwrap()
            .action,
        "adjust:1"
    );
    assert_eq!(
        host.settings_hit(claude.previous_column, claude.control_line)
            .unwrap()
            .action,
        "adjust:-1"
    );
    assert_eq!(
        host.settings_hit(claude.open_column, claude.control_line)
            .unwrap()
            .action,
        "open"
    );
    let prompt = host.settings_row_geometry("custom_prompt");
    assert_eq!(
        host.settings_hit(10, prompt.first_line).unwrap().action,
        "adjust:0"
    );
    let model = host.settings_row_geometry("model");
    assert_eq!(
        host.settings_hit(10, model.first_line).unwrap().action,
        "select"
    );
    assert!(host.settings_hit(200, 0).is_none());
    let scrolled = host.settings_hit_after_scroll(2, 0).expect("hit");
    assert_eq!(scrolled.row, "enabled");
    assert_ne!(
        host.settings_rows().first().map(String::as_str),
        Some("privacy_banner")
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::rendered_remote_controls_use_shared_choice_and_centered_number_layouts
#[test]
fn rsui_rendered_remote_controls_use_shared_choice_and_centered_number_layouts() {
    let host = dismissed();
    for row in ["threshold", "tail_tokens", "keep_backups"] {
        let geometry = host.settings_row_geometry(row);
        let painted = host.settings_painted_control(row, 110);
        for glyph in ['-', '+', '*'] {
            assert!(painted.contains(glyph), "{row}: {painted:?}");
        }
        assert!(geometry.value_slot_width > geometry.value_width, "{row}");
    }
    for row in ["technique:claude", "technique:codex", "technique:other"] {
        let painted = host.settings_painted_control(row, 110);
        for glyph in ['\u{2190}', '+', '\u{2192}'] {
            assert!(painted.contains(glyph), "{row}: {painted:?}");
        }
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::settings_screen_paints_and_hits_remote_controls_in_the_same_geometry
#[test]
fn rsui_settings_screen_paints_and_hits_remote_controls_in_the_same_geometry() {
    let host = dismissed();
    for row in ["threshold", "pause_timeout", "keep_backups"] {
        let geometry = host.settings_row_geometry(row);
        assert!(geometry.previous_column < geometry.value_column, "{row}");
        assert!(geometry.value_right <= geometry.next_column, "{row}");
        assert!(geometry.next_column < geometry.open_column, "{row}");
        assert_eq!(
            host.settings_painted_glyph(geometry.next_column, geometry.control_line),
            '+',
            "{row}"
        );
        assert_eq!(
            host.settings_hit(geometry.next_column, geometry.control_line)
                .unwrap()
                .action,
            "adjust:1"
        );
    }
    for row in ["technique:claude", "technique:codex"] {
        let geometry = host.settings_row_geometry(row);
        assert!(geometry.previous_column < geometry.value_column, "{row}");
        assert!(geometry.value_right <= geometry.open_column, "{row}");
        assert!(geometry.open_column < geometry.next_column, "{row}");
        assert_eq!(
            host.settings_hit(geometry.open_column, geometry.control_line)
                .unwrap()
                .action,
            "open"
        );
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::scroll_follows_the_selection_and_the_page_scrolls_to_its_end
#[test]
fn rsui_scroll_follows_the_selection_and_the_page_scrolls_to_its_end() {
    let host = dismissed();
    let height = 12;
    assert_eq!(host.settings_scroll_for_selection(height, 0, 0), 0);
    let last = host.settings_rows().len() - 1;
    let scroll = host.settings_scroll_for_selection(height, last, 0);
    assert!(scroll > 0);
    let span = host.settings_row_span(last);
    assert!(span.last_line < scroll + height);
    assert!(scroll <= host.settings_max_scroll(last, height));
    assert!(host.settings_scroll_for_selection(height, 0, scroll) < scroll);
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::every_row_has_a_help_anchor_on_its_first_line
#[test]
fn rsui_every_row_has_a_help_anchor_on_its_first_line() {
    let host = tab();
    let anchors = host.settings_help_anchors();
    assert_eq!(anchors.len(), host.settings_rows().len());
    for (row, (id, line)) in host.settings_rows().iter().zip(anchors) {
        assert_eq!(id, host.settings_row_help_id(row), "{row}");
        assert!(line > 0, "{row}");
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::a_pending_numeric_save_is_visible_in_the_title
#[test]
fn rsui_a_pending_numeric_save_is_visible_in_the_title() {
    let mut host = dismissed();
    assert!(!host.settings_page_text(110).contains("saving\u{2026}"));
    host.settings_adjust_row("threshold", 1);
    assert!(host.settings_page_text(110).contains("saving\u{2026}"));
}

/// REQ: PORT:ilium-client/src/remote_compaction_settings_ui.rs::narrow_widths_never_panic_and_keep_the_banner_inside_the_page
#[test]
fn rsui_narrow_widths_never_panic_and_keep_the_banner_inside_the_page() {
    let host = tab();
    for width in [1u16, 2, 3, 5, 8, 12, 20, 30, 40, 60] {
        let lines = host.settings_page_lines(width);
        assert!(!lines.is_empty(), "width {width}");
        for line in &lines {
            assert!(
                line.chars().count() <= usize::from(width),
                "width {width}: {line:?}"
            );
        }
        if let Some(close) = host.settings_banner_close_rect_at(width) {
            assert!(close.column + close.width <= width, "width {width}");
        }
    }
}
