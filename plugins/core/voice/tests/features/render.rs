//! The two renders of the page: the voice strip while listening (panel `voice`) and the confirmation dialog
//! before a voice command closes a pane (popup `confirm-voice`, kind danger-confirm). Goldens at 48 columns are
//! the page figures; other widths are checked structurally.

use ilium_plugin_sdk::testing::{FakeHost, assert_golden};
use ilium_plugin_sdk::types::{AudioFramesEvent, Event, ViewTarget};
use proptest::prelude::*;

use crate::common::{
    CONFIRM_POPUP, add_pane, enabled_client, f8_grab, golden, hear, invoked_commands, key_pressed,
};

const STRIP_ROWS: u16 = 5;
const CONFIRM_ROWS: u16 = 6;

fn strip() -> ViewTarget {
    ViewTarget::Panel("voice".to_string())
}

fn confirm() -> ViewTarget {
    ViewTarget::Popup(CONFIRM_POPUP.to_string())
}

fn listening_host() -> (FakeHost, u64) {
    let mut host = enabled_client("push_to_talk");
    let grab = f8_grab(&mut host);
    host.deliver_one(key_pressed(grab, false));
    host.settle();
    let capture = host.audio().captures().first().expect("capture").id;
    (host, capture)
}

fn confirming_host(pane: &str) -> FakeHost {
    let mut host = enabled_client("push_to_talk");
    add_pane(&mut host, pane);
    hear(&mut host, &format!("close the pane called {pane}"));
    host
}

/// REQ: F-050
#[test]
fn render_050_the_strip_title_and_listening_state() {
    let (mut host, _capture) = listening_host();
    let text = host.render(strip(), 48, STRIP_ROWS).text();
    assert!(text.lines().next().expect("row").starts_with("\u{256d}\u{2500} Voice "), "{text}");
    assert!(text.contains("\u{25cf} Listening"), "{text}");
}

/// REQ: F-050
#[test]
fn render_050_the_strip_hint_row_names_release_and_escape() {
    let (mut host, _capture) = listening_host();
    let text = host.render(strip(), 48, STRIP_ROWS).text();
    assert!(text.contains("F8 release to send \u{b7} Esc cancel"), "{text}");
}

/// REQ: F-050
#[test]
fn render_050_the_strip_has_five_rows_inside_a_rounded_border() {
    let (mut host, _capture) = listening_host();
    let screen = host.render(strip(), 48, STRIP_ROWS);
    assert_eq!(screen.rows, STRIP_ROWS);
    assert!(screen.row(0).starts_with('\u{256d}') && screen.row(0).ends_with('\u{256e}'));
    assert!(screen.row(STRIP_ROWS - 1).starts_with('\u{2570}') && screen.row(STRIP_ROWS - 1).ends_with('\u{256f}'));
}

/// REQ: F-050
#[test]
fn render_050_the_level_meter_shows_bar_glyphs_once_frames_arrive() {
    let (mut host, capture) = listening_host();
    host.deliver_one(Event::AudioFrames(AudioFramesEvent { capture, samples: vec![12_000, -12_000, 9_000, -9_000] }));
    host.settle();
    let meter_row = host.render(strip(), 48, STRIP_ROWS).row(1);
    assert!(meter_row.chars().any(|glyph| "\u{2581}\u{2582}\u{2583}\u{2584}\u{2585}\u{2586}\u{2587}".contains(glyph)), "{meter_row}");
}

/// REQ: F-050
#[test]
fn render_050_silence_shows_no_taller_bar_than_the_lowest() {
    let (mut host, capture) = listening_host();
    host.deliver_one(Event::AudioFrames(AudioFramesEvent { capture, samples: vec![0; 64] }));
    host.settle();
    let meter_row = host.render(strip(), 48, STRIP_ROWS).row(1);
    assert!(
        meter_row.chars().filter(|glyph| "\u{2581}\u{2582}\u{2583}\u{2584}\u{2585}\u{2586}\u{2587}".contains(*glyph)).all(|glyph| glyph == '\u{2581}'),
        "{meter_row}"
    );
}

/// REQ: F-050
#[test]
fn render_050_escape_on_the_strip_cancels_the_recording_without_a_job() {
    let (mut host, _capture) = listening_host();
    host.key(strip(), "Esc");
    host.settle();
    assert!(host.audio().captures().iter().all(|capture| !capture.open));
    assert!(host.jobs().list().is_empty());
}

/// REQ: F-050
#[test]
fn render_050_the_strip_is_not_shown_while_voice_is_idle() {
    let mut host = enabled_client("push_to_talk");
    let text = host.render(strip(), 48, STRIP_ROWS).text();
    assert!(!text.contains("Listening"), "{text}");
}

/// REQ: F-051
#[test]
fn render_051_the_confirmation_at_48_columns_equals_the_page_figure() {
    let mut host = confirming_host("docs");
    let screen = host.render(confirm(), 48, CONFIRM_ROWS);
    assert_golden(&golden("voice_confirm_48.txt"), &screen.text());
}

/// REQ: F-051
#[test]
fn render_051_the_confirmation_names_what_the_user_said_and_what_will_close() {
    let mut host = confirming_host("docs");
    let text = host.render(confirm(), 48, CONFIRM_ROWS).text();
    assert!(text.contains("You said: close the pane called docs"));
    assert!(text.contains("Pane docs and its terminal will close."));
}

/// REQ: F-051
#[test]
fn render_051_the_buttons_are_escape_cancel_and_enter_close_pane() {
    let mut host = confirming_host("docs");
    let text = host.render(confirm(), 48, CONFIRM_ROWS).text();
    assert!(text.contains("Esc Cancel"));
    assert!(text.contains("Enter Close pane"));
    let last_content = text.lines().nth(4).expect("button row");
    assert!(last_content.find("Esc Cancel") < last_content.find("Enter Close pane"));
}

/// REQ: F-051
#[test]
fn render_051_the_confirmation_title_is_close_pane_question() {
    let mut host = confirming_host("docs");
    let screen = host.render(confirm(), 48, CONFIRM_ROWS);
    assert!(screen.row(0).starts_with("\u{256d}\u{2500} Close pane? "), "{}", screen.row(0));
}

/// REQ: F-051
#[test]
fn render_051_the_dialog_is_structurally_the_same_at_40_80_and_120_columns() {
    for columns in [40_u16, 80, 120] {
        let mut host = confirming_host("docs");
        let screen = host.render(confirm(), columns, CONFIRM_ROWS);
        let text = screen.text();
        assert!(text.contains("Close pane?"), "{columns}: {text}");
        assert!(text.contains("Esc Cancel") && text.contains("Enter Close pane"), "{columns}: {text}");
        assert!(screen.row(0).starts_with('\u{256d}'), "{columns}");
        assert!(screen.row(CONFIRM_ROWS - 1).starts_with('\u{2570}'), "{columns}");
    }
}

/// REQ: F-051
#[test]
fn render_051_the_popup_is_gone_once_the_dialog_is_answered() {
    let mut host = confirming_host("docs");
    host.deliver_one(crate::common::popup_button(CONFIRM_POPUP, "cancel"));
    host.settle();
    assert!(host.popups().is_empty());
    assert!(invoked_commands(&host.submitted()).is_empty());
}

proptest! {
    // REQ: F-051
    #[test]
    fn render_051_property_the_pane_name_appears_in_both_dialog_lines(name in "[a-z][a-z0-9]{0,11}") {
        let mut host = confirming_host(&name);
        let text = host.render(confirm(), 48, CONFIRM_ROWS).text();
        let said = format!("You said: close the pane called {name}");
        let will_close = format!("Pane {name} and its terminal will close.");
        prop_assert!(text.contains(&said), "{}", text);
        prop_assert!(text.contains(&will_close), "{}", text);
    }
}
