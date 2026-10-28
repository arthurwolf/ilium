//! Twins of ilium/tests/pty_tui_smoke.rs: last prompt banner behaviour in a real terminal.
//! Request/reply shape: see API_GAPS_ports.md.
use crate::common::{ask, Place};
use serde_json::Value;

fn check(place: Place, op: &str, request: &str, expected: &str) {
    let request: Value = serde_json::from_str(request).expect("request literal parses");
    let expected: Value = serde_json::from_str(expected).expect("expected literal parses");
    let reply = ask(place, op, request);
    let fields = expected.as_object().expect("expected is an object");
    for (name, value) in fields {
        assert_eq!(
            &reply[name.as_str()],
            value,
            "field {name} of reply {reply}"
        );
    }
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::last_prompt_banner_keeps_fixed_slot_for_a_short_prompt
#[test]
fn port_pty_tui_smoke_last_prompt_banner_keeps_fixed_slot_for_a_short_prompt_alternate() {
    check(
        Place::Client,
        "banner.pty",
        r####"{"prompt": ""}"####,
        r####"{"banner_rows_reserved": 4, "content_starts_after_rows": 4}"####,
    );
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::last_prompt_banner_splits_a_bracketed_paste_on_lone_carriage_returns
#[test]
fn port_pty_tui_smoke_last_prompt_banner_splits_a_bracketed_paste_on_lone_carriage_returns_alternate(
) {
    check(
        Place::Client,
        "banner.pty",
        r####"{"paste": "one\rtwo"}"####,
        r####"{"banner_rows": ["one", "two"], "last_prompt": "one\rtwo"}"####,
    );
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::last_prompt_banner_updates_from_ordinary_typed_keystrokes_with_a_correction
#[test]
fn port_pty_tui_smoke_last_prompt_banner_updates_from_ordinary_typed_keystrokes_with_a_correction_alternate(
) {
    check(
        Place::Client,
        "banner.pty",
        r####"{"keys": "ab\u007f"}"####,
        r####"{"last_prompt": "a"}"####,
    );
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::last_prompt_banner_keeps_fixed_slot_for_a_short_prompt
#[test]
fn port_pty_tui_smoke_last_prompt_banner_keeps_fixed_slot_for_a_short_prompt() {
    check(
        Place::Client,
        "banner.pty",
        r####"{"prompt": "hi"}"####,
        r####"{"banner_below_row": "⏹ Stop", "content_starts_after_rows": 4, "content": "Pursuing goal"}"####,
    );
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::last_prompt_banner_splits_a_bracketed_paste_on_lone_carriage_returns
#[test]
fn port_pty_tui_smoke_last_prompt_banner_splits_a_bracketed_paste_on_lone_carriage_returns() {
    check(
        Place::Client,
        "banner.pty",
        r####"{"paste": "first line\rsecond line\rthird line\rfourth line\rfifth line\rsixth line"}"####,
        r####"{"banner_rows": ["first line", "second line", "fifth line", "sixth line"], "last_prompt": "first line\rsecond line\rthird line\rfourth line\rfifth line\rsixth line"}"####,
    );
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::last_prompt_banner_updates_from_ordinary_typed_keystrokes_with_a_correction
#[test]
fn port_pty_tui_smoke_last_prompt_banner_updates_from_ordinary_typed_keystrokes_with_a_correction()
{
    check(
        Place::Client,
        "banner.pty",
        r####"{"keys": "fix the testzs"}"####,
        r####"{"last_prompt": "fix the tests"}"####,
    );
}
