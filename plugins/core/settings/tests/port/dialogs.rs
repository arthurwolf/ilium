//! Twins of ilium-client/src/popover.rs, startup_dialog.rs and modal.rs. The pop-ups are views
//! the plugin renders (`ViewTarget::Popup(<id>)`); hover and key actions arrive through the
//! harness and the fake clock (200 ms hover delay, 250 ms leave grace).

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};

use crate::common::*;

fn launcher() -> ViewTarget {
    ViewTarget::Popup("agent-launcher".to_string())
}

fn hover(host: &mut ilium_plugin_sdk::testing::FakeHost, kind: ActionKind) {
    host.send_action(launcher(), "anchor", kind, ActionValue::None);
}

fn visible(host: &mut ilium_plugin_sdk::testing::FakeHost) -> bool {
    !host.render(launcher(), 80, 24).text().trim().is_empty()
}

/// REQ: PORT:ilium-client/src/popover.rs::crossing_directly_between_button_and_popover_preserves_preview, F-090
#[test]
fn port_090_moving_from_the_button_onto_the_popover_keeps_the_preview() {
    let mut host = client_host();
    hover(&mut host, ActionKind::HoverEnter);
    host.advance_clock(200);
    host.settle();
    host.send_action(
        launcher(),
        "surface",
        ActionKind::HoverEnter,
        ActionValue::None,
    );
    hover(&mut host, ActionKind::HoverLeave);
    host.advance_clock(250);
    host.settle();
    assert!(visible(&mut host));
}

/// REQ: PORT:ilium-client/src/popover.rs::hover_delay_and_leave_grace_are_independent, F-090
#[test]
fn port_090_the_preview_shows_at_200_ms_and_not_at_199_ms() {
    let mut host = client_host();
    hover(&mut host, ActionKind::HoverEnter);
    host.advance_clock(199);
    host.settle();
    assert!(!visible(&mut host));
    host.advance_clock(1);
    host.settle();
    assert!(visible(&mut host));
}

/// REQ: PORT:ilium-client/src/popover.rs::hover_delay_and_leave_grace_are_independent, F-090
#[test]
fn port_090_the_preview_closes_250_ms_after_the_pointer_left_and_not_at_249_ms() {
    let mut host = client_host();
    hover(&mut host, ActionKind::HoverEnter);
    host.advance_clock(200);
    host.settle();
    hover(&mut host, ActionKind::HoverLeave);
    host.advance_clock(249);
    host.settle();
    assert!(visible(&mut host));
    host.advance_clock(1);
    host.settle();
    assert!(!visible(&mut host));
}

/// REQ: PORT:ilium-client/src/popover.rs::leaving_before_hover_delay_never_opens_preview, F-090
#[test]
fn port_090_leaving_after_50_ms_never_opens_the_preview() {
    let mut host = client_host();
    hover(&mut host, ActionKind::HoverEnter);
    host.advance_clock(50);
    hover(&mut host, ActionKind::HoverLeave);
    host.advance_clock(150);
    host.settle();
    assert!(!visible(&mut host));
    host.advance_clock(250);
    host.settle();
    assert!(!visible(&mut host));
}

/// REQ: PORT:ilium-client/src/popover.rs::narrow_sidebar_has_one_row_with_clickable_choices, F-090
#[test]
fn port_090_a_24_column_sidebar_shows_the_choices_on_one_row() {
    let mut host = client_host();
    hover(&mut host, ActionKind::HoverEnter);
    host.advance_clock(200);
    host.settle();
    let screen = host.render(launcher(), 24, 24);
    let lines: Vec<String> = (0..24)
        .map(|row| screen.row(row))
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
}

/// REQ: PORT:ilium-client/src/popover.rs::narrow_sidebar_has_one_row_with_clickable_choices, F-090
#[test]
fn port_090_clicking_a_choice_on_the_narrow_row_is_handled() {
    let mut host = client_host();
    hover(&mut host, ActionKind::HoverEnter);
    host.advance_clock(200);
    host.settle();
    let screen = host.render(launcher(), 24, 24);
    let row = (0..24)
        .find(|row| !screen.row(*row).trim().is_empty())
        .expect("one row");
    let column = screen
        .row(row)
        .chars()
        .position(|c| !c.is_whitespace())
        .expect("a choice") as u16;
    assert!(host.click(launcher(), column, row));
}

/// REQ: PORT:ilium-client/src/popover.rs::pinning_opens_immediately_and_does_not_expire, F-090
#[test]
fn port_090_pinning_shows_the_menu_at_once_and_it_does_not_expire() {
    let mut host = client_host();
    host.send_action(
        launcher(),
        "anchor",
        ActionKind::Activated,
        ActionValue::None,
    );
    host.settle();
    assert!(visible(&mut host));
    hover(&mut host, ActionKind::HoverLeave);
    host.advance_clock(2000);
    host.settle();
    assert!(visible(&mut host));
}

/// REQ: PORT:ilium-client/src/popover.rs::returning_after_an_aborted_hover_starts_a_fresh_delay, F-090
#[test]
fn port_090_returning_after_an_aborted_hover_waits_a_fresh_200_ms() {
    let mut host = client_host();
    hover(&mut host, ActionKind::HoverEnter);
    host.advance_clock(50);
    hover(&mut host, ActionKind::HoverLeave);
    host.advance_clock(130);
    hover(&mut host, ActionKind::HoverEnter);
    host.advance_clock(199);
    host.settle();
    assert!(!visible(&mut host));
    host.advance_clock(1);
    host.settle();
    assert!(visible(&mut host));
}

fn startup() -> ViewTarget {
    ViewTarget::Popup("startup-progress".to_string())
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::composed_startup_progress_overlay_is_visible_at_terminal_sizes, F-089
#[test]
fn port_089_the_startup_overlay_shows_title_category_detail_and_bar_at_four_sizes() {
    for (width, height) in [(24u16, 10u16), (40, 12), (80, 24), (120, 40)] {
        let mut host = client_host();
        let text = host.render(startup(), width, height).text();
        assert!(text.contains("Ilium"), "{width}x{height}");
        assert!(text.contains("Loading the session"), "{width}x{height}");
        assert!(text.contains("Receiving panes"), "{width}x{height}");
        assert!(text.contains('\u{2591}'), "{width}x{height}");
    }
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::every_row_is_exactly_the_dialog_width, F-089
#[test]
fn port_089_every_dialog_row_has_the_same_width() {
    for width in [24u16, 40, 64] {
        let mut host = client_host();
        let screen = host.render(startup(), width + 4, 7);
        let widths: Vec<usize> = (0..7)
            .map(|row| screen.row(row).trim().chars().count())
            .filter(|count| *count > 0)
            .collect();
        assert!(
            widths.windows(2).all(|pair| pair[0] == pair[1]),
            "{width}: {widths:?}"
        );
    }
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::known_progress_fills_proportionally_and_labels_percent, F-089
#[test]
fn port_089_half_progress_labels_fifty_percent_and_fills_half_the_bar() {
    let mut host = client_host();
    host.send_action(
        startup(),
        "progress",
        ActionKind::Changed,
        ActionValue::Number(0.5),
    );
    host.settle();
    let text = host.render(startup(), 100, 30).text();
    assert!(text.contains(" 50%"), "{text}");
    let line = text
        .lines()
        .find(|line| line.contains('\u{2588}'))
        .expect("bar row");
    let filled = line.chars().filter(|c| *c == '\u{2588}').count();
    let empty = line.chars().filter(|c| *c == '\u{2591}').count();
    assert!(
        filled > 0 && empty > 0 && filled.abs_diff(empty) <= 1,
        "{line}"
    );
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::small_terminals_get_no_dialog_and_large_ones_get_it_centred, F-089
#[test]
fn port_089_a_terminal_below_24_columns_or_7_rows_gets_no_dialog() {
    let mut host = client_host();
    assert!(host.render(startup(), 20, 30).text().trim().is_empty());
    assert!(host.render(startup(), 100, 5).text().trim().is_empty());
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::small_terminals_get_no_dialog_and_large_ones_get_it_centred, F-089
#[test]
fn port_089_a_100_column_terminal_centres_a_64_column_dialog() {
    let mut host = client_host();
    let screen = host.render(startup(), 100, 30);
    let row = (0..30)
        .find(|row| screen.row(*row).contains('\u{256d}'))
        .expect("top border");
    let line = screen.row(row);
    assert_eq!(line.chars().position(|c| c == '\u{256d}'), Some(18));
    assert_eq!(line.trim().chars().count(), 64);
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::top_border_uses_shared_ilium_chrome_title, F-089
#[test]
fn port_089_the_top_border_carries_the_shared_chrome_title() {
    for width in [24u16, 40, 64] {
        let mut host = client_host();
        let screen = host.render(startup(), width + 4, 7);
        let line = (0..7)
            .map(|row| screen.row(row))
            .find(|line| line.contains('\u{256d}'))
            .expect("top");
        let line = line.trim();
        assert!(
            line.starts_with("\u{256d}  \u{2261} \u{25cf} \u{b7} Ilium \u{2500}"),
            "{line:?}"
        );
        assert!(line.ends_with('\u{256e}'));
        assert_eq!(line.chars().count(), usize::from(width));
    }
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::unknown_progress_moves_between_ticks, F-089
#[test]
fn port_089_an_unknown_progress_bar_moves_between_ticks() {
    let mut host = client_host();
    let first = host.render(startup(), 100, 30).text();
    host.advance_clock(900);
    host.settle();
    let later = host.render(startup(), 100, 30).text();
    assert_ne!(first, later);
}

fn confirm() -> ViewTarget {
    ViewTarget::Popup("confirm".to_string())
}

/// REQ: PORT:ilium-client/src/modal.rs::compact_buttons_keep_complete_labels_before_shortcut_hints, F-090
#[test]
fn port_090_the_create_button_keeps_its_label_and_shows_enter_only_from_18_columns() {
    for width in [16u16, 24] {
        let mut host = client_host();
        let text = host
            .render(ViewTarget::Popup("form-actions".to_string()), width, 1)
            .text();
        assert!(text.contains("Create board"), "{width}: {text:?}");
        assert_eq!(text.contains("Enter"), width >= 18, "{width}: {text:?}");
    }
}

/// REQ: PORT:ilium-client/src/modal.rs::compact_text_prompt_uses_a_complete_hint_that_fits, F-090
#[test]
fn port_090_the_compact_prompt_hint_is_complete_and_fits_the_row() {
    let mut host = client_host();
    let screen = host.render(ViewTarget::Popup("text-prompt".to_string()), 40, 12);
    let hint = (0..12)
        .map(|row| screen.row(row))
        .find(|line| line.contains("Esc cancel"))
        .expect("hint row");
    assert!(hint.contains("Enter Apply"), "{hint:?}");
    assert!(hint.trim().chars().count() <= 40);
}

/// REQ: PORT:ilium-client/src/modal.rs::confirmation_keyboard_hint_adapts_to_available_width, F-090
#[test]
fn port_090_the_confirmation_hint_adapts_to_the_width() {
    for width in [24u16, 30, 40, 60, 80] {
        let mut host = client_host();
        let text = host.render(confirm(), width, 12).text();
        assert!(
            text.replace(' ', "").contains("Y/N"),
            "width {width}: {text}"
        );
        if width >= 60 {
            assert!(
                text.contains("Enter confirm") && text.contains("Esc cancel"),
                "{width}"
            );
        }
        if width >= 80 {
            assert!(text.contains("Click either button"), "{width}");
        }
    }
}

/// REQ: PORT:ilium-client/src/modal.rs::confirmation_layout_exposes_two_distinct_mouse_targets_on_small_screens, F-090
#[test]
fn port_090_a_32_by_8_confirmation_has_two_separate_clickable_buttons() {
    let mut host = client_host();
    let screen = host.render(confirm(), 32, 8);
    let row = (0..8)
        .find(|row| screen.row(*row).contains("Cancel"))
        .expect("button row");
    let line = screen.row(row);
    let cancel = line.find("Cancel").expect("cancel") as u16;
    let ok = line
        .rfind("Close")
        .or_else(|| line.rfind("Confirm"))
        .expect("confirm") as u16;
    assert!(cancel + 6 <= ok);
    assert!(host.click(confirm(), cancel, row));
    assert!(host.click(confirm(), ok, row));
}

/// REQ: PORT:ilium-client/src/modal.rs::destructive_confirmation_renders_aerated_named_buttons_with_dark_safe_tones, F-090
#[test]
fn port_090_the_destructive_confirmation_names_both_buttons() {
    let mut host = client_host();
    let text = host.render(confirm(), 100, 30).text();
    assert!(text.contains("Keep open"), "{text}");
    assert!(text.contains("Close"), "{text}");
}

/// REQ: PORT:ilium-client/src/modal.rs::destructive_confirmation_renders_aerated_named_buttons_with_dark_safe_tones, F-090
#[test]
fn port_090_the_destructive_button_tone_differs_from_the_neutral_cancel_tone() {
    let mut host = client_host();
    let screen = host.render(confirm(), 100, 30);
    let (row, column) = screen.find("Close").expect("close button");
    let (keep_row, keep_column) = screen.find("Keep open").expect("keep button");
    let danger = screen.cell(row, column).background;
    let neutral = screen.cell(keep_row, keep_column).background;
    assert_ne!(
        danger, neutral,
        "tones differ: the destructive button is red, the neutral one dark grey"
    );
}

/// REQ: PORT:ilium-client/src/modal.rs::text_prompt_renders_clickable_cancel_and_primary_actions, F-090
#[test]
fn port_090_the_text_prompt_shows_cancel_and_a_distinct_primary_action() {
    let mut host = client_host();
    let screen = host.render(ViewTarget::Popup("text-prompt".to_string()), 100, 30);
    assert!(screen.text().contains("Cancel"));
    let (row, column) = screen.find("Rename").expect("primary");
    let (cancel_row, cancel_column) = screen.find("Cancel").expect("cancel");
    assert_ne!(
        screen.cell(row, column).background,
        screen.cell(cancel_row, cancel_column).background
    );
}

/// REQ: PORT:ilium-client/src/popover.rs::crossing_directly_between_button_and_popover_preserves_preview, F-090
#[test]
fn port_090_the_preview_closes_250_ms_after_the_pointer_leaves_the_popover() {
    let mut host = client_host();
    hover(&mut host, ActionKind::HoverEnter);
    host.advance_clock(200);
    host.settle();
    host.send_action(launcher(), "surface", ActionKind::HoverEnter, ActionValue::None);
    host.send_action(launcher(), "surface", ActionKind::HoverLeave, ActionValue::None);
    host.advance_clock(250);
    host.settle();
    assert!(!visible(&mut host));
}

/// REQ: PORT:ilium-client/src/popover.rs::leaving_before_hover_delay_never_opens_preview, F-090
#[test]
fn port_090_a_click_still_opens_the_menu_after_an_aborted_hover() {
    let mut host = client_host();
    hover(&mut host, ActionKind::HoverEnter);
    host.advance_clock(50);
    hover(&mut host, ActionKind::HoverLeave);
    host.advance_clock(150);
    host.settle();
    host.send_action(launcher(), "anchor", ActionKind::Activated, ActionValue::None);
    host.settle();
    assert!(visible(&mut host));
}

/// REQ: PORT:ilium-client/src/popover.rs::pinning_opens_immediately_and_does_not_expire, F-090
#[test]
fn port_090_a_pinned_menu_survives_the_pointer_leaving_for_250_ms() {
    let mut host = client_host();
    host.send_action(launcher(), "anchor", ActionKind::Activated, ActionValue::None);
    host.settle();
    hover(&mut host, ActionKind::HoverLeave);
    host.advance_clock(250);
    host.settle();
    assert!(visible(&mut host));
}

/// REQ: PORT:ilium-client/src/popover.rs::returning_after_an_aborted_hover_starts_a_fresh_delay, F-090
#[test]
fn port_090_an_aborted_hover_timer_never_opens_the_preview_later() {
    let mut host = client_host();
    hover(&mut host, ActionKind::HoverEnter);
    host.advance_clock(50);
    hover(&mut host, ActionKind::HoverLeave);
    host.advance_clock(300);
    host.settle();
    assert!(!visible(&mut host));
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::composed_startup_progress_overlay_is_visible_at_terminal_sizes, F-089
#[test]
fn port_089_no_startup_overlay_row_is_wider_than_a_small_terminal() {
    for (width, height) in [(24u16, 10u16), (40, 12)] {
        let mut host = client_host();
        let text = host.render(startup(), width, height).text();
        assert!(
            text.lines().all(|line| line.chars().count() <= usize::from(width)),
            "{width}x{height}: {text}"
        );
    }
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::every_row_is_exactly_the_dialog_width, F-089
#[test]
fn port_089_the_bottom_border_is_closed_at_both_corners() {
    let mut host = client_host();
    let screen = host.render(startup(), 40, 7);
    let row = (0..7)
        .find(|row| screen.row(*row).contains('\u{2570}'))
        .expect("bottom border");
    let line = screen.row(row);
    let line = line.trim();
    assert!(line.starts_with('\u{2570}'), "{line:?}");
    assert!(line.ends_with('\u{256f}'), "{line:?}");
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::known_progress_fills_proportionally_and_labels_percent, F-089
#[test]
fn port_089_complete_progress_fills_the_whole_bar_and_labels_100_percent() {
    let mut host = client_host();
    host.send_action(
        startup(),
        "progress",
        ActionKind::Changed,
        ActionValue::Number(1.0),
    );
    host.settle();
    let text = host.render(startup(), 100, 30).text();
    assert!(text.contains("100%"), "{text}");
    let line = text
        .lines()
        .find(|line| line.contains('\u{2588}'))
        .expect("bar row");
    assert!(!line.contains('\u{2591}'), "{line}");
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::top_border_uses_shared_ilium_chrome_title, F-089
#[test]
fn port_089_the_top_border_fills_the_space_after_its_title_with_rules() {
    let mut host = client_host();
    let screen = host.render(startup(), 40, 7);
    let line = (0..7)
        .map(|row| screen.row(row))
        .find(|line| line.contains('\u{256d}'))
        .expect("top");
    assert!(line.contains("\u{2500}\u{2500}"), "{line:?}");
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::unknown_progress_moves_between_ticks, F-089
#[test]
fn port_089_an_unknown_progress_bar_shows_no_percent_label() {
    let mut host = client_host();
    let text = host.render(startup(), 100, 30).text();
    assert!(!text.contains('%'), "{text}");
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::small_terminals_get_no_dialog_and_large_ones_get_it_centred, F-089
#[test]
fn port_089_a_24_by_7_terminal_is_the_smallest_that_gets_a_dialog() {
    let mut host = client_host();
    assert!(!host.render(startup(), 24, 7).text().trim().is_empty());
}

/// REQ: PORT:ilium-client/src/startup_dialog.rs::small_terminals_get_no_dialog_and_large_ones_get_it_centred, F-089
#[test]
fn port_089_a_dialog_in_a_64_column_terminal_starts_at_column_zero() {
    let mut host = client_host();
    let screen = host.render(startup(), 64, 10);
    let row = (0..10)
        .find(|row| screen.row(*row).contains('\u{256d}'))
        .expect("top border");
    let line = screen.row(row);
    assert_eq!(line.chars().position(|c| c == '\u{256d}'), Some(0));
}

/// REQ: PORT:ilium-client/src/modal.rs::compact_buttons_keep_complete_labels_before_shortcut_hints, F-090
#[test]
fn port_090_the_create_button_shows_its_enter_hint_at_40_columns() {
    let mut host = client_host();
    let text = host
        .render(ViewTarget::Popup("form-actions".to_string()), 40, 1)
        .text();
    assert!(text.contains("Create board"), "{text}");
    assert!(text.contains("Enter"), "{text}");
}

/// REQ: PORT:ilium-client/src/modal.rs::compact_text_prompt_uses_a_complete_hint_that_fits, F-090
#[test]
fn port_090_the_prompt_hint_stays_on_a_single_row_at_wide_sizes() {
    let mut host = client_host();
    let screen = host.render(ViewTarget::Popup("text-prompt".to_string()), 100, 12);
    let hints = (0..12)
        .filter(|row| screen.row(*row).contains("Esc cancel"))
        .count();
    assert_eq!(hints, 1);
}

/// REQ: PORT:ilium-client/src/modal.rs::confirmation_keyboard_hint_adapts_to_available_width, F-090
#[test]
fn port_090_the_mouse_hint_is_dropped_on_a_narrow_confirmation() {
    let mut host = client_host();
    let text = host.render(confirm(), 24, 12).text();
    assert!(!text.contains("Click either button"), "{text}");
}

/// REQ: PORT:ilium-client/src/modal.rs::confirmation_layout_exposes_two_distinct_mouse_targets_on_small_screens, F-090
#[test]
fn port_090_a_click_on_the_dialog_border_hits_no_button() {
    let mut host = client_host();
    assert!(!host.click(confirm(), 0, 0));
}

/// REQ: PORT:ilium-client/src/modal.rs::destructive_confirmation_renders_aerated_named_buttons_with_dark_safe_tones, F-090
#[test]
fn port_090_both_confirmation_buttons_stay_named_at_60_columns() {
    let mut host = client_host();
    let text = host.render(confirm(), 60, 30).text();
    assert!(text.contains("Keep open"), "{text}");
    assert!(text.contains("Close"), "{text}");
}

/// REQ: PORT:ilium-client/src/modal.rs::destructive_confirmation_renders_aerated_named_buttons_with_dark_safe_tones, F-090
#[test]
fn port_090_the_destructive_button_background_is_uniform_across_its_label() {
    let mut host = client_host();
    let screen = host.render(confirm(), 100, 30);
    let (row, column) = screen.find("Close").expect("close button");
    assert_eq!(
        screen.cell(row, column).background,
        screen.cell(row, column + 1).background
    );
}

/// REQ: PORT:ilium-client/src/modal.rs::text_prompt_renders_clickable_cancel_and_primary_actions, F-090
#[test]
fn port_090_the_text_prompt_keeps_cancel_and_rename_at_40_columns() {
    let mut host = client_host();
    let text = host
        .render(ViewTarget::Popup("text-prompt".to_string()), 40, 12)
        .text();
    assert!(text.contains("Cancel"), "{text}");
    assert!(text.contains("Rename"), "{text}");
}
