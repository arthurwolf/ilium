//! api-settings.md, "Dynamic content": `set-info`, `set-info-rich`, `set-row-note`,
//! `set-row-state`, `set-item-states` and `set-meter`. The calls need no grant, apply to the
//! plugin's own rows, return `not-found` for an unknown key or id, `invalid-argument` for a row
//! of the wrong kind or a value over a limit, and `rate-limited` above 10 calls per second.
//!
//! Observation accessors on the fixture are listed in API_GAPS_p2.md.

use std::time::Duration;

use ilium_plugin_api::generated::host::{HostSettings, ItemState};
use ilium_plugin_api::generated::types::{
    ErrorCode, MeterSegment, RichSpan, RowState, RowStateKind, SpanTarget, Style, Tone,
};
use ilium_plugin_api::Tier;
use ilium_plugin_host::testing::imports_fixture::ImportsFixture;
use proptest::prelude::*;
use serde_json::json;

use crate::support::*;

fn dynamic_manifest() -> serde_json::Value {
    manifest_with_rows(vec![
        toggle("enabled", true),
        text("label", 40, "work"),
        info("status"),
        json!({ "type": "info", "id": "plain", "title": "plain", "text": "fixed" }),
        json!({ "type": "info", "id": "who", "title": "who", "text": "x", "live": true,
                "rich": [ { "text": "start" } ] }),
        json!({ "type": "list", "key": "queue", "title": "Queue",
                "item": { "type": "string", "max_length": 40 }, "max_items": 8,
                "default": [], "remote": true }),
        json!({ "type": "list", "key": "names", "title": "Names",
                "item": { "type": "string", "max_length": 40 }, "max_items": 8,
                "default": [] }),
        meter("usage"),
    ])
}

fn dynamic_fixture() -> ImportsFixture {
    ImportsFixture::builder()
        .tier(Tier::T2)
        .manifest_json(&dynamic_manifest().to_string())
        .build()
}

fn repeat_char(character: char, count: usize) -> String {
    std::iter::repeat(character).take(count).collect()
}

fn state(kind: RowStateKind, message: Option<&str>) -> RowState {
    RowState {
        kind,
        message: message.map(str::to_owned),
    }
}

fn span(text: &str) -> RichSpan {
    RichSpan {
        text: text.to_owned(),
        style: Style::default(),
        target: SpanTarget::None,
        tooltip: None,
    }
}

fn segment(label: &str, value: f64, tone: Tone) -> MeterSegment {
    MeterSegment {
        label: label.to_owned(),
        value,
        tone,
    }
}

fn segments(count: usize) -> Vec<MeterSegment> {
    (0..count)
        .map(|index| segment(&format!("s{index}"), index as f64, Tone::Info))
        .collect()
}

// ---- set-info ----------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/settings.set-info
#[test]
fn settings_set_info_replaces_the_text_of_a_live_info_row() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_info("status".to_owned(), "3 files".to_owned()));
    assert_eq!(fixture.info_text("status").as_deref(), Some("3 files"));
}

/// REQ: WIT:ilium:host/settings.set-info
#[test]
fn settings_set_info_second_call_replaces_the_first() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_info("status".to_owned(), "one".to_owned()));
    expect_ok(fixture.settings().set_info("status".to_owned(), "two".to_owned()));
    assert_eq!(fixture.info_text("status").as_deref(), Some("two"));
}

/// REQ: WIT:ilium:host/settings.set-info
#[test]
fn settings_set_info_120_characters_accepted() {
    let mut fixture = dynamic_fixture();
    let text = repeat_char('a', 120);
    expect_ok(fixture.settings().set_info("status".to_owned(), text.clone()));
    assert_eq!(fixture.info_text("status"), Some(text));
}

/// REQ: WIT:ilium:host/settings.set-info
#[test]
fn settings_set_info_121_characters_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(
        fixture
            .settings()
            .set_info("status".to_owned(), repeat_char('a', 121)),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-info
#[test]
fn settings_set_info_counts_characters_not_bytes() {
    let mut fixture = dynamic_fixture();
    let text = repeat_char('\u{e9}', 120);
    assert_eq!(text.len(), 240);
    expect_ok(fixture.settings().set_info("status".to_owned(), text));
}

/// REQ: WIT:ilium:host/settings.set-info
#[test]
fn settings_set_info_over_the_limit_keeps_the_previous_text() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_info("status".to_owned(), "kept".to_owned()));
    let _ = fixture
        .settings()
        .set_info("status".to_owned(), repeat_char('a', 121));
    assert_eq!(fixture.info_text("status").as_deref(), Some("kept"));
}

/// REQ: WIT:ilium:host/settings.set-info
#[test]
fn settings_set_info_empty_text_accepted() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_info("status".to_owned(), String::new()));
    assert_eq!(fixture.info_text("status").as_deref(), Some(""));
}

/// REQ: WIT:ilium:host/settings.set-info
#[test]
fn settings_set_info_unknown_key_not_found() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_info("nope".to_owned(), "x".to_owned()));
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/settings.set-info
#[test]
fn settings_set_info_on_an_info_row_without_live_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_info("plain".to_owned(), "x".to_owned()));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-info
#[test]
fn settings_set_info_on_a_value_row_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_info("label".to_owned(), "x".to_owned()));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

proptest! {
    /// REQ: WIT:ilium:host/settings.set-info
    #[test]
    fn settings_set_info_accepts_exactly_up_to_120_characters(length in 0usize..260) {
        let mut fixture = dynamic_fixture();
        let result = fixture
            .settings()
            .set_info("status".to_owned(), repeat_char('x', length));
        if length <= 120 {
            prop_assert!(result.is_ok());
        } else {
            prop_assert_eq!(result.err().map(|error| error.code), Some(ErrorCode::InvalidArgument));
        }
    }
}

// ---- set-info-rich -----------------------------------------------------------------------------

/// REQ: WIT:ilium:host/settings.set-info-rich
#[test]
fn settings_set_info_rich_replaces_the_spans_of_a_live_rich_row() {
    let mut fixture = dynamic_fixture();
    let spans = vec![span("Signed in as "), span("ada")];
    expect_ok(fixture.settings().set_info_rich("who".to_owned(), spans.clone()));
    assert_eq!(fixture.info_spans("who"), spans);
}

/// REQ: WIT:ilium:host/settings.set-info-rich
#[test]
fn settings_set_info_rich_keeps_the_tone_and_command_of_a_span() {
    let mut fixture = dynamic_fixture();
    let mut clickable = span("sign out");
    clickable.target = SpanTarget::Command("sign-out".to_owned());
    expect_ok(fixture.settings().set_info_rich("who".to_owned(), vec![clickable.clone()]));
    assert_eq!(fixture.info_spans("who"), vec![clickable]);
}

/// REQ: WIT:ilium:host/settings.set-info-rich
#[test]
fn settings_set_info_rich_120_characters_in_total_accepted() {
    let mut fixture = dynamic_fixture();
    let spans = vec![span(&repeat_char('a', 60)), span(&repeat_char('b', 60))];
    expect_ok(fixture.settings().set_info_rich("who".to_owned(), spans));
}

/// REQ: WIT:ilium:host/settings.set-info-rich
#[test]
fn settings_set_info_rich_121_characters_in_total_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let spans = vec![span(&repeat_char('a', 60)), span(&repeat_char('b', 61))];
    let code = expect_code(fixture.settings().set_info_rich("who".to_owned(), spans));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-info-rich
#[test]
fn settings_set_info_rich_unknown_key_not_found() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_info_rich("nope".to_owned(), vec![span("x")]));
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/settings.set-info-rich
#[test]
fn settings_set_info_rich_on_an_info_row_without_rich_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_info_rich("status".to_owned(), vec![span("x")]));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-info-rich
#[test]
fn settings_set_info_rich_is_rate_limited_above_10_calls_per_second() {
    let mut fixture = dynamic_fixture();
    for _ in 0..10 {
        expect_ok(fixture.settings().set_info_rich("who".to_owned(), vec![span("x")]));
    }
    let error = expect_error(fixture.settings().set_info_rich("who".to_owned(), vec![span("x")]));
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert!(error.retry_after_ms.is_some());
}

// ---- set-row-note ------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/settings.set-row-note
#[test]
fn settings_set_row_note_sets_a_note_under_a_value_row() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_row_note("label".to_owned(), Some("check this".to_owned())));
    assert_eq!(fixture.row_note("label").as_deref(), Some("check this"));
}

/// REQ: WIT:ilium:host/settings.set-row-note
#[test]
fn settings_set_row_note_none_clears_the_note() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_row_note("label".to_owned(), Some("a".to_owned())));
    expect_ok(fixture.settings().set_row_note("label".to_owned(), None));
    assert_eq!(fixture.row_note("label"), None);
}

/// REQ: WIT:ilium:host/settings.set-row-note
#[test]
fn settings_set_row_note_applies_to_any_row_kind() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_row_note("status".to_owned(), Some("n".to_owned())));
    expect_ok(fixture.settings().set_row_note("usage".to_owned(), Some("m".to_owned())));
    assert_eq!(fixture.row_note("status").as_deref(), Some("n"));
    assert_eq!(fixture.row_note("usage").as_deref(), Some("m"));
}

/// REQ: WIT:ilium:host/settings.set-row-note
#[test]
fn settings_set_row_note_120_characters_accepted() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_row_note("label".to_owned(), Some(repeat_char('n', 120))));
}

/// REQ: WIT:ilium:host/settings.set-row-note
#[test]
fn settings_set_row_note_121_characters_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(
        fixture
            .settings()
            .set_row_note("label".to_owned(), Some(repeat_char('n', 121))),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-row-note
#[test]
fn settings_set_row_note_unknown_key_not_found() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_row_note("nope".to_owned(), Some("x".to_owned())));
    assert_eq!(code, ErrorCode::NotFound);
}

// ---- set-row-state -----------------------------------------------------------------------------

/// REQ: WIT:ilium:host/settings.set-row-state
#[test]
fn settings_set_row_state_each_of_the_four_kinds_is_applied() {
    let mut fixture = dynamic_fixture();
    for kind in [
        RowStateKind::Loading,
        RowStateKind::Pending,
        RowStateKind::Error,
        RowStateKind::Idle,
    ] {
        fixture.advance(Duration::from_secs(1));
        let wanted = state(kind, None);
        expect_ok(fixture.settings().set_row_state("label".to_owned(), wanted.clone()));
        assert_eq!(fixture.row_state("label"), wanted);
    }
}

/// REQ: WIT:ilium:host/settings.set-row-state
#[test]
fn settings_set_row_state_message_is_kept() {
    let mut fixture = dynamic_fixture();
    let wanted = state(RowStateKind::Loading, Some("checking the key"));
    expect_ok(fixture.settings().set_row_state("label".to_owned(), wanted.clone()));
    assert_eq!(fixture.row_state("label"), wanted);
}

/// REQ: WIT:ilium:host/settings.set-row-state
#[test]
fn settings_set_row_state_message_of_80_characters_accepted() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_row_state(
        "label".to_owned(),
        state(RowStateKind::Error, Some(&repeat_char('m', 80))),
    ));
}

/// REQ: WIT:ilium:host/settings.set-row-state
#[test]
fn settings_set_row_state_message_of_81_characters_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_row_state(
        "label".to_owned(),
        state(RowStateKind::Error, Some(&repeat_char('m', 81))),
    ));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-row-state
#[test]
fn settings_set_row_state_unknown_key_not_found() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(
        fixture
            .settings()
            .set_row_state("nope".to_owned(), state(RowStateKind::Idle, None)),
    );
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/settings.set-row-state
#[test]
fn settings_set_row_state_on_a_meter_row_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(
        fixture
            .settings()
            .set_row_state("usage".to_owned(), state(RowStateKind::Loading, None)),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-row-state
#[test]
fn settings_set_row_state_10_calls_per_second_accepted_and_the_11th_rate_limited() {
    let mut fixture = dynamic_fixture();
    for _ in 0..10 {
        expect_ok(
            fixture
                .settings()
                .set_row_state("label".to_owned(), state(RowStateKind::Pending, None)),
        );
    }
    let error = expect_error(
        fixture
            .settings()
            .set_row_state("label".to_owned(), state(RowStateKind::Pending, None)),
    );
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert!(error.retry_after_ms.is_some());
}

/// REQ: WIT:ilium:host/settings.set-row-state
#[test]
fn settings_set_row_state_is_accepted_again_after_the_window_passes() {
    let mut fixture = dynamic_fixture();
    for _ in 0..10 {
        expect_ok(
            fixture
                .settings()
                .set_row_state("label".to_owned(), state(RowStateKind::Pending, None)),
        );
    }
    assert!(fixture
        .settings()
        .set_row_state("label".to_owned(), state(RowStateKind::Pending, None))
        .is_err());
    fixture.advance(Duration::from_millis(1100));
    expect_ok(
        fixture
            .settings()
            .set_row_state("label".to_owned(), state(RowStateKind::Idle, None)),
    );
}

// ---- set-item-states ---------------------------------------------------------------------------

/// REQ: WIT:ilium:host/settings.set-item-states
#[test]
fn settings_set_item_states_sets_the_state_of_single_items() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_json(r#"{"queue": ["a", "b", "c"]}"#.to_owned()));
    let items = vec![
        ItemState { index: 0, state: state(RowStateKind::Pending, None) },
        ItemState { index: 2, state: state(RowStateKind::Error, Some("refused")) },
    ];
    expect_ok(fixture.settings().set_item_states("queue".to_owned(), items));
    assert_eq!(fixture.item_state("queue", 0), Some(state(RowStateKind::Pending, None)));
    assert_eq!(fixture.item_state("queue", 2), Some(state(RowStateKind::Error, Some("refused"))));
}

/// REQ: WIT:ilium:host/settings.set-item-states
#[test]
fn settings_set_item_states_leaves_unnamed_items_unchanged() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_json(r#"{"queue": ["a", "b"]}"#.to_owned()));
    expect_ok(fixture.settings().set_item_states(
        "queue".to_owned(),
        vec![ItemState { index: 0, state: state(RowStateKind::Loading, None) }],
    ));
    assert_ne!(fixture.item_state("queue", 1), Some(state(RowStateKind::Loading, None)));
}

/// REQ: WIT:ilium:host/settings.set-item-states
#[test]
fn settings_set_item_states_empty_list_accepted() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_item_states("queue".to_owned(), Vec::new()));
}

/// REQ: WIT:ilium:host/settings.set-item-states
#[test]
fn settings_set_item_states_unknown_key_not_found() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_item_states("nope".to_owned(), Vec::new()));
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/settings.set-item-states
#[test]
fn settings_set_item_states_on_a_list_without_remote_or_item_status_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_item_states(
        "names".to_owned(),
        vec![ItemState { index: 0, state: state(RowStateKind::Pending, None) }],
    ));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-item-states
#[test]
fn settings_set_item_states_on_a_toggle_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_item_states("enabled".to_owned(), Vec::new()));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-item-states
#[test]
fn settings_set_item_states_index_past_the_last_item_invalid_argument() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_json(r#"{"queue": ["a"]}"#.to_owned()));
    let code = expect_code(fixture.settings().set_item_states(
        "queue".to_owned(),
        vec![ItemState { index: 1, state: state(RowStateKind::Pending, None) }],
    ));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-item-states
#[test]
fn settings_set_item_states_is_rate_limited_above_10_calls_per_second() {
    let mut fixture = dynamic_fixture();
    for _ in 0..10 {
        expect_ok(fixture.settings().set_item_states("queue".to_owned(), Vec::new()));
    }
    let code = expect_code(fixture.settings().set_item_states("queue".to_owned(), Vec::new()));
    assert_eq!(code, ErrorCode::RateLimited);
}

// ---- set-meter ---------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/settings.set-meter
#[test]
fn settings_set_meter_replaces_the_segments() {
    let mut fixture = dynamic_fixture();
    let wanted = vec![segment("used", 40.0, Tone::Success), segment("free", 60.0, Tone::Muted)];
    expect_ok(fixture.settings().set_meter("usage".to_owned(), wanted.clone()));
    assert_eq!(fixture.meter_segments("usage"), wanted);
}

/// REQ: WIT:ilium:host/settings.set-meter
#[test]
fn settings_set_meter_second_call_replaces_the_first() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_meter("usage".to_owned(), segments(3)));
    expect_ok(fixture.settings().set_meter("usage".to_owned(), segments(1)));
    assert_eq!(fixture.meter_segments("usage"), segments(1));
}

/// REQ: WIT:ilium:host/settings.set-meter
#[test]
fn settings_set_meter_8_segments_accepted() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_meter("usage".to_owned(), segments(8)));
    assert_eq!(fixture.meter_segments("usage").len(), 8);
}

/// REQ: WIT:ilium:host/settings.set-meter
#[test]
fn settings_set_meter_9_segments_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_meter("usage".to_owned(), segments(9)));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-meter
#[test]
fn settings_set_meter_empty_list_clears_the_meter() {
    let mut fixture = dynamic_fixture();
    expect_ok(fixture.settings().set_meter("usage".to_owned(), segments(2)));
    expect_ok(fixture.settings().set_meter("usage".to_owned(), Vec::new()));
    assert!(fixture.meter_segments("usage").is_empty());
}

/// REQ: WIT:ilium:host/settings.set-meter
#[test]
fn settings_set_meter_unknown_id_not_found() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_meter("nope".to_owned(), segments(1)));
    assert_eq!(code, ErrorCode::NotFound);
}

/// REQ: WIT:ilium:host/settings.set-meter
#[test]
fn settings_set_meter_on_a_row_that_is_not_a_meter_invalid_argument() {
    let mut fixture = dynamic_fixture();
    let code = expect_code(fixture.settings().set_meter("status".to_owned(), segments(1)));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/settings.set-meter
#[test]
fn settings_set_meter_is_rate_limited_above_10_calls_per_second() {
    let mut fixture = dynamic_fixture();
    for _ in 0..10 {
        expect_ok(fixture.settings().set_meter("usage".to_owned(), segments(1)));
    }
    let code = expect_code(fixture.settings().set_meter("usage".to_owned(), segments(1)));
    assert_eq!(code, ErrorCode::RateLimited);
}

/// REQ: WIT:ilium:host/settings.set-meter
#[test]
fn settings_row_update_calls_share_one_limit_of_10_per_second() {
    let mut fixture = dynamic_fixture();
    for _ in 0..5 {
        expect_ok(fixture.settings().set_meter("usage".to_owned(), segments(1)));
        expect_ok(
            fixture
                .settings()
                .set_row_state("label".to_owned(), state(RowStateKind::Idle, None)),
        );
    }
    let code = expect_code(fixture.settings().set_meter("usage".to_owned(), segments(1)));
    assert_eq!(code, ErrorCode::RateLimited);
}
