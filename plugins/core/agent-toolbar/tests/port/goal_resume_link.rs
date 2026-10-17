//! Twins of goal_resume_link.rs: the clickable /goal resume link found in an agent footer line.
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

/// REQ: PORT:ilium-client/src/goal_resume_link.rs::finds_command_in_footer_with_exact_columns
#[test]
fn port_goal_resume_link_finds_command_in_footer_with_exact_columns() {
    check(
        Place::Client,
        "goal-resume.find",
        r####"{"rows": 6, "columns": 60, "lines": {"2": "  gpt Paused · /goal resume · ok"}}"####,
        r####"{"row": 2, "start_column": 15, "width": 12, "contains_start": true, "contains_end": false}"####,
    );
}

/// REQ: PORT:ilium-client/src/goal_resume_link.rs::ignores_transcript_text_above_the_footer
#[test]
fn port_goal_resume_link_ignores_transcript_text_above_the_footer() {
    check(
        Place::Client,
        "goal-resume.find",
        r####"{"rows": 6, "columns": 60, "lines": {"0": "/goal resume", "2": "  gpt Paused"}}"####,
        r####"{"link": null}"####,
    );
}

/// REQ: PORT:ilium-client/src/goal_resume_link.rs::paints_blue_underline_and_hover_variant
#[test]
fn port_goal_resume_link_paints_blue_underline_and_hover_variant() {
    check(
        Place::Client,
        "goal-resume.paint",
        r####"{"hover": false}"####,
        r####"{"normal": {"fg": "blue", "underline": true}, "hover": {"fg": "light-blue", "bold": true}}"####,
    );
}

/// REQ: PORT:ilium-client/src/goal_resume_link.rs::rejects_longer_tokens
#[test]
fn port_goal_resume_link_rejects_longer_tokens() {
    check(
        Place::Client,
        "goal-resume.find",
        r####"{"rows": 6, "columns": 60, "lines": {"2": "  gpt Paused · /goal resumed · ok"}}"####,
        r####"{"link": null}"####,
    );
}
